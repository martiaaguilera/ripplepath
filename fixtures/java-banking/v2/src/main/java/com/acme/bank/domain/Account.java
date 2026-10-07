package com.acme.bank.domain;

import com.acme.bank.api.ApiErrors;

public class Account {
    private final String id;
    private Money balance;
    private boolean frozen;

    public Account(String id, Money openingBalance) {
        this.id = id;
        this.balance = openingBalance;
    }

    public String id() {
        return id;
    }

    public Money balance() {
        return balance;
    }

    public void withdraw(Money amount) {
        if (frozen) {
            throw new IllegalStateException(ApiErrors.accountFrozen(id));
        }
        Money next = balance.minus(amount);
        if (next.isNegative()) {
            throw new InsufficientFundsException(id);
        }
        balance = next;
    }

    public void deposit(Money amount) {
        balance = balance.plus(amount);
    }

    public void freeze() {
        frozen = true;
    }
}
