;; Execute after reset, including when the rule network was restored from a snapshot.
(assert (bag (id singleton) (left d) (right e)))
(assert (bag (id several) (left d) (right e f)))
(assert (bag (id empty) (left) (right)))
(assert (bag (id omitted)))
