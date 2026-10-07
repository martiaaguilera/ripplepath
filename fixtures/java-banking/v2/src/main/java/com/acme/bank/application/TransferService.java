package com.acme.bank.application;

import com.acme.bank.domain.Account;
import com.acme.bank.domain.FeePolicy;
import com.acme.bank.domain.Money;
import com.acme.bank.persistence.AccountRepository;

public class TransferService {
    private final AccountRepository accounts;
    private final FeePolicy feePolicy;

    public TransferService(AccountRepository accounts, FeePolicy feePolicy) {
        this.accounts = accounts;
        this.feePolicy = feePolicy;
    }

    public void transfer(String fromId, String toId, Money amount) {
        Account from = load(fromId);
        Account to = load(toId);
        Money fee = feePolicy.feeFor(amount);
        from.withdraw(amount.plus(fee));
        to.deposit(amount);
        accounts.save(from);
        accounts.save(to);
    }

    private Account load(String id) {
        return accounts.findById(id, true).orElseThrow(() -> new IllegalArgumentException("unknown account " + id));
    }
}
