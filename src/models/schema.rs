// @generated automatically by Diesel CLI.

diesel::table! {
    bets (id) {
        id -> Int4,
        oracle_announcement -> Bytea,
        user_a -> Bytea,
        win_a -> Jsonb,
        lose_a -> Jsonb,
        user_b -> Bytea,
        win_b -> Jsonb,
        lose_b -> Jsonb,
        oracle_event_id -> Bytea,
        needs_reply -> Bool,
        win_outcome_event_id -> Nullable<Bytea>,
        lose_outcome_event_id -> Nullable<Bytea>,
        created_at -> Timestamp,
    }
}

diesel::table! {
    sigs (id) {
        id -> Int4,
        bet_id -> Int4,
        is_party_a -> Bool,
        is_win -> Bool,
        sig -> Bytea,
        outcome -> Text,
    }
}

diesel::joinable!(sigs -> bets (bet_id));

diesel::allow_tables_to_appear_in_same_query!(
    bets,
    sigs,
);
