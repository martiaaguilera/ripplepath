package com.acme.bank.domain;

public class StandardFeePolicy implements FeePolicy {
    @Override
    public Money feeFor(Money amount) {
        if (amount.currency().equals("EUR")) {
            return Money.of("0.25", amount.currency());
        }
        return Money.of("0.50", amount.currency());
    }
}
