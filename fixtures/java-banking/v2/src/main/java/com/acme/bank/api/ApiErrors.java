package com.acme.bank.api;

public final class ApiErrors {
    private ApiErrors() {
    }

    public static String accountFrozen(String accountId) {
        return "ACCOUNT_FROZEN:" + accountId;
    }
}
