package com.acme.bank.domain;

public class InsufficientFundsException extends RuntimeException {
    public InsufficientFundsException(String accountId) {
        super("insufficient funds in " + accountId);
    }
}
