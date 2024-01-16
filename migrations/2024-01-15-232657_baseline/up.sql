CREATE TABLE bets
(
    id                  SERIAL PRIMARY KEY,
    oracle_announcement bytea     NOT NULL,
    user_a              bytea     NOT NULL,
    unsigned_a          jsonb     NOT NULL,
    user_b              bytea     NOT NULL,
    unsigned_b          jsonb     NOT NULL,
    oracle_event_id     bytea     NOT NULL,
    needs_reply         BOOLEAN   NOT NULL DEFAULT TRUE,
    outcome_event_id    bytea,
    created_at          TIMESTAMP NOT NULL DEFAULT NOW()
);

create index bets_user_a_idx on bets (user_a);
create index bets_user_b_idx on bets (user_b);
create index beta_oracle_event_id_idx on bets (oracle_event_id);

CREATE TABLE sigs
(
    id         SERIAL PRIMARY KEY,
    bet_id     integer NOT NULL,
    is_party_a boolean NOT NULL,
    sig        bytea   NOT NULL,
    outcome    TEXT    NOT NULL,
    FOREIGN KEY (bet_id) REFERENCES bets (id)
);

create unique index sigs_bet_id_outcome_idx on sigs (bet_id, outcome);
create index sigs_bet_id_idx on sigs (bet_id);
