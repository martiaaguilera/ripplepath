package com.acme.bank.domain;

public class StandardFeePolicy implements FeePolicy {
    @Override
    public Money feeFor(Money amount) {
        return Money.of("0.50", amount.currency());
    }
}
