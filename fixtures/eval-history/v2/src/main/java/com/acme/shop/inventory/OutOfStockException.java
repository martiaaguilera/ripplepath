package com.acme.shop.inventory;

public final class OutOfStockException extends RuntimeException {
    public OutOfStockException(String sku, int requested, int available) {
        super("cannot reserve " + requested + " x " + sku + ": only " + available + " available");
    }
}
