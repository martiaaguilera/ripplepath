package com.acme.bank.api;

import com.acme.bank.application.TransferService;
import com.acme.bank.domain.Money;

public class TransferController {
    private final TransferService transfers;

    public TransferController(TransferService transfers) {
        this.transfers = transfers;
    }

    public String transfer(String fromId, String toId, String amount, String currency) {
        transfers.transfer(fromId, toId, Money.of(amount, currency));
        return "OK";
    }
}
