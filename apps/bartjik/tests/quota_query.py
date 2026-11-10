"""Latest weekly quota selection, including legacy windows and timestamp ties."""
import sqlite3
from pathlib import Path
c = sqlite3.connect(':memory:')
c.executescript('''
CREATE TABLE accounts(id TEXT PRIMARY KEY, alias TEXT, email TEXT, plan_type TEXT, status TEXT);
CREATE TABLE usage_history(id INTEGER PRIMARY KEY, account_id TEXT, window TEXT,
 window_minutes INTEGER, used_percent REAL, reset_at INTEGER, recorded_at INTEGER);
CREATE INDEX idx_usage_account_time ON usage_history(account_id, recorded_at);
INSERT INTO accounts VALUES ('a','Alpha','a@example.test','PLUS','active'),
 ('b','','b@example.test','PRO','active'), ('c','Hidden','c@example.test','PLUS','disabled'),
 ('d','Empty','d@example.test','PLUS','active');
INSERT INTO usage_history VALUES
 (1,'a','secondary',NULL,10,100,1),
 (2,'a','primary',10080,20,200,2),
 (3,'a','secondary',10080,30,300,2),
 (4,'a','primary',300,99,400,3),
 (5,'a','secondary',300,99,400,4),
 (6,'b','secondary',NULL,40,500,1),
 (7,'b','primary',NULL,99,600,2),
 (8,'c','secondary',10080,50,700,1);
''')
query = (Path(__file__).resolve().parents[1] / 'src/quota.sql').read_text()
assert c.execute(query).fetchall() == [
    ('a', 'Alpha', 'plus', 'weekly', 70.0, 300),
    ('b', 'b@example.test', 'pro', 'weekly', 60.0, 500),
]
print('PASS latest weekly quota, legacy classification, ties and inactive/missing accounts')
