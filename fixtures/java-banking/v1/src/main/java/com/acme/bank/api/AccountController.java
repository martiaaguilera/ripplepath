package com.acme.bank.api;

import com.acme.bank.domain.Account;
import com.acme.bank.persistence.AccountRepository;

public class AccountController {
    private final AccountRepository accounts;

    public AccountController(AccountRepository accounts) {
        this.accounts = accounts;
    }

    public String balance(String id) {
        Account account = accounts.findById(id).orElseThrow(() -> new IllegalArgumentException(id));
        return account.balance().toString();
    }

    public String legacyBalance(String id) {
        return balance(id);
    }
}
