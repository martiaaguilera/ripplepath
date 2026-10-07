package com.acme.bank.application;

import static org.junit.jupiter.api.Assertions.assertEquals;

import com.acme.bank.domain.Account;
import com.acme.bank.domain.Money;
import com.acme.bank.domain.StandardFeePolicy;
import com.acme.bank.persistence.InMemoryAccountRepository;
import org.junit.jupiter.api.Test;

class TransferServiceTest {
    @Test
    void movesMoneyAndChargesFee() {
        InMemoryAccountRepository repo = new InMemoryAccountRepository();
        repo.save(new Account("a", Money.of("10.00", "EUR")));
        repo.save(new Account("b", Money.of("0.00", "EUR")));
        TransferService service = new TransferService(repo, new StandardFeePolicy());

        service.transfer("a", "b", Money.of("5.00", "EUR"));

        assertEquals("EUR", repo.findById("b", false).get().balance().currency());
    }
}
