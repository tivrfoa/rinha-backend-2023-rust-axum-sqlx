CREATE TABLE IF NOT EXISTS PESSOAS (
    ID VARCHAR(36) PRIMARY KEY,
    APELIDO VARCHAR(32) UNIQUE NOT NULL,
    NOME VARCHAR(100) NOT NULL,
    NASCIMENTO CHAR(10) NOT NULL,
    STACK VARCHAR(1024),
    BUSCA_TRGM TEXT GENERATED ALWAYS AS (
        LOWER(NOME || APELIDO || STACK)
    ) STORED
);

CREATE EXTENSION IF NOT EXISTS PG_TRGM;
CREATE INDEX CONCURRENTLY IF NOT EXISTS IDX_PESSOAS_BUSCA_TGRM ON PESSOAS USING GIST (BUSCA_TRGM GIST_TRGM_OPS);

