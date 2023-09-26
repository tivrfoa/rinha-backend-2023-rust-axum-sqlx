
-- https://dev.mysql.com/doc/refman/8.0/en/fulltext-natural-language.html

CREATE TABLE IF NOT EXISTS pessoas (
    id VARCHAR(36) PRIMARY KEY,
    apelido VARCHAR(32) unique not null,
    nome VARCHAR(100) not null,
    nascimento CHAR(10) not null,
    stack VARCHAR(500),
    FULLTEXT(apelido, nome, stack)
);



/*

insert into pessoas
values (
    "a",
    "joao",
    "joao santos",
    "2000-01-20",
    "C Rust"
);


select *
from pessoas
where match(apelido, nome, nascimento) against('ntos'  IN NATURAL LANGUAGE MODE);


docker start some-mysql



$ docker exec -it some-mysql /bin/bash
bash-4.4# mysql -p
Enter password: 
Welcome to the MySQL monitor.  Commands end with ; or \g.
Your MySQL connection id is 8
Server version: 8.1.0 MySQL Community Server - GPL

Copyright (c) 2000, 2023, Oracle and/or its affiliates.

Oracle is a registered trademark of Oracle Corporation and/or its
affiliates. Other names may be trademarks of their respective
owners.

Type 'help;' or '\h' for help. Type '\c' to clear the current input statement.

mysql> show databases;
+--------------------+
| Database           |
+--------------------+
| information_schema |
| mysql              |
| performance_schema |
| rinhadb            |
| sys                |
+--------------------+
5 rows in set (0.01 sec)

mysql> use rinhadb;
Reading table information for completion of table and column names
You can turn off this feature to get a quicker startup with -A

Database changed
mysql> show tables;
+-------------------+
| Tables_in_rinhadb |
+-------------------+
| PESSOAS           |
+-------------------+
1 row in set (0.00 sec)

mysql> drop table pessoas;
ERROR 1051 (42S02): Unknown table 'rinhadb.pessoas'
mysql> drop table PESSOAS;
Query OK, 0 rows affected (0.02 sec)

mysql>

*/

/*

# Read Later

https://dev.mysql.com/doc/refman/8.0/en/innodb-introduction.html

https://hub.docker.com/_/mysql

https://dev.mysql.com/doc/refman/8.0/en/char.html

https://dev.mysql.com/doc/refman/8.0/en/fulltext-search-ngram.html



*/
