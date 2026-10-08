import { SUPPORTED_SCHEMA_VERSION, type AnalysisReport, type FileBlob, type Health } from "./types";

export class ApiError extends Error {
  constructor(
    message: string,
    readonly status: number,
  ) {
    super(message);
  }
}

async function getJson<T>(url: string, signal?: AbortSignal): Promise<T> {
  const response = await fetch(url, signal ? { signal } : {});
  const body: unknown = await response.json().catch(() => null);
  if (!response.ok) {
    const message =
      typeof body === "object" && body !== null && "error" in body
        ? String((body as { error: { message?: unknown } }).error.message)
        : `request failed with status ${response.status}`;
    throw new ApiError(message, response.status);
  }
  return body as T;
}

export function fetchHealth(signal?: AbortSignal): Promise<Health> {
  return getJson<Health>("/api/v1/health", signal);
}

export async function fetchAnalysis(base: string, head: string, signal?: AbortSignal): Promise<AnalysisReport> {
  const params = new URLSearchParams({ base, head });
  const report = await getJson<AnalysisReport>(`/api/v1/analysis?${params.toString()}`, signal);
  if (report.schema_version !== SUPPORTED_SCHEMA_VERSION) {
    // Rendering a report whose field meanings may have changed would be silently wrong.
    throw new ApiError(
      `server returned schema v${report.schema_version}; this UI understands v${SUPPORTED_SCHEMA_VERSION}`,
      200,
    );
  }
  return report;
}

// Blobs are immutable for a given commit, so a small cache makes flipping between files in the
// diff view instant without any invalidation logic. Bounded: a long session must not hoard source.
const FILE_CACHE_LIMIT = 48;
const fileCache = new Map<string, Promise<FileBlob>>();

/** One file of a revision. `rev` should be a commit or tree id so the result matches the report. */
export function fetchFile(rev: string, path: string): Promise<FileBlob> {
  const key = `${rev}\u0000${path}`;
  const cached = fileCache.get(key);
  if (cached) return cached;
  const params = new URLSearchParams({ rev, path });
  const request = getJson<FileBlob>(`/api/v1/file?${params.toString()}`);
  fileCache.set(key, request);
  // Failures are not cached: a transient error must not stick for the rest of the session.
  request.catch(() => fileCache.delete(key));
  if (fileCache.size > FILE_CACHE_LIMIT) {
    const oldest = fileCache.keys().next();
    if (!oldest.done) fileCache.delete(oldest.value);
  }
  return request;
}
