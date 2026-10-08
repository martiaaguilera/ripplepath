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
        int available = available(sku);
        if (quantity > available) {
            throw new OutOfStockException(sku, quantity, available);
        }
        stock.put(sku, available - quantity);
    }
}
