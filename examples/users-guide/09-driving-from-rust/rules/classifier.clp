(defrule decide-vip
    (declare (salience 100))
    (user-tier vip)
    =>
    (assert (decision premium))
    (assert (decided)))

(defrule decide-warn
    (declare (salience 50))
    (has-crashed yes)
    (not (decided))
    =>
    (assert (decision warn))
    (assert (decided)))

(defrule decide-default
    (declare (salience 10))
    (initial-fact)
    (not (decided))
    =>
    (assert (decision standard))
    (assert (decided)))
