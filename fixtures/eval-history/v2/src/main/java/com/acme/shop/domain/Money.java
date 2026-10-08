package com.acme.shop.domain;

/** An amount in euro cents. Immutable. */
public final class Money implements Comparable<Money> {
    public static final Money ZERO = new Money(0);

    private final long cents;

    public Money(long cents) {
        if (cents < 0) {
            throw new IllegalArgumentException("negative amount: " + cents);
        }
        this.cents = cents;
    }

    public static Money ofCents(long cents) {
        return new Money(cents);
    }

    public long cents() {
        return cents;
    }

    public Money plus(Money other) {
        return new Money(cents + other.cents);
    }

    /** Never below zero: a discount larger than the amount leaves nothing to pay. */
    public Money minus(Money other) {
        return new Money(Math.max(0, cents - other.cents));
    }

    public Money times(int quantity) {
        return new Money(cents * quantity);
    }

    /** {@code percent}% of this amount, rounded half up to the cent. */
    public Money percent(int percent) {
        return new Money((cents * percent + 50) / 100);
    }

    @Override
    public int compareTo(Money other) {
        return Long.compare(cents, other.cents);
    }

    @Override
    public boolean equals(Object o) {
        return o instanceof Money m && m.cents == cents;
    }

    @Override
    public int hashCode() {
        return Long.hashCode(cents);
    }

    @Override
    public String toString() {
        return String.format("%d.%02d EUR", cents / 100, cents % 100);
    }
}
