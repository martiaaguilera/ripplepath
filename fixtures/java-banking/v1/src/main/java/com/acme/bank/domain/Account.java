package com.acme.bank.domain;

public class Account {
    private final String id;
    private Money balance;

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
        Money next = balance.minus(amount);
        if (next.isNegative()) {
            throw new InsufficientFundsException(id);
        }
        balance = next;
    }

    public void deposit(Money amount) {
        balance = balance.plus(amount);
    }
}
