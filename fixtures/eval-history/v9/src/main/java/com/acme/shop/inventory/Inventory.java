package com.acme.shop.inventory;

import java.util.Map;
import java.util.TreeMap;

public final class Inventory {
    private final Map<String, Integer> stock = new TreeMap<>();

    public void restock(String sku, int quantity) {
        stock.merge(sku, quantity, Integer::sum);
    }

    public int available(String sku) {
        return stock.getOrDefault(sku, 0);
    }

    public void reserve(String sku, int quantity) {
        int remaining = requireAvailable(sku, quantity);
        stock.put(sku, remaining);
    }

    /** The stock left after taking {@code quantity}; refuses to go below zero. */
    private int requireAvailable(String sku, int quantity) {
        int onHand = available(sku);
        if (quantity <= 0 || quantity >= onHand) {
            throw new OutOfStockException(sku, quantity, onHand);
        }
        return onHand - quantity;
    }
}
