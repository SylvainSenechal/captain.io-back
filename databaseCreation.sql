-- cat databaseCreation.sql | sqlite3 game.db

CREATE TABLE IF NOT EXISTS Players (
    uuid TEXT NOT NULL PRIMARY KEY,
    name TEXT NOT NULL UNIQUE
) WITHOUT ROWID;