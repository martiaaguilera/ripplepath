package com.acme.bank.persistence;

import com.acme.bank.domain.Account;
import java.util.Optional;

public interface AccountRepository {
    Optional<Account> findById(String id, boolean forUpdate);

    void save(Account account);
}
