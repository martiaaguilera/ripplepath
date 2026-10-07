package com.acme.bank.api;

import static org.junit.jupiter.api.Assertions.assertEquals;

import com.acme.bank.application.TransferService;
import com.acme.bank.domain.Account;
import com.acme.bank.domain.Money;
import com.acme.bank.domain.StandardFeePolicy;
import com.acme.bank.persistence.InMemoryAccountRepository;
import org.junit.jupiter.api.Test;

class TransferControllerTest {
    @Test
    void returnsOkOnSuccessfulTransfer() {
        InMemoryAccountRepository repo = new InMemoryAccountRepository();
        repo.save(new Account("a", Money.of("10.00", "EUR")));
        repo.save(new Account("b", Money.of("0.00", "EUR")));
        TransferController controller =
                new TransferController(new TransferService(repo, new StandardFeePolicy()));

        assertEquals("OK", controller.transfer("a", "b", "1.00", "EUR"));
    }
}
