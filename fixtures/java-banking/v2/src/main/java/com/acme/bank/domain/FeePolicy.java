package com.acme.bank.domain;

public interface FeePolicy {
    Money feeFor(Money amount);
}
