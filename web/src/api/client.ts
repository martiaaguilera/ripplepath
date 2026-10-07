import { SUPPORTED_SCHEMA_VERSION, type AnalysisReport, type Health } from "./types";

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
