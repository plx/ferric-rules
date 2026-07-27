; Assert-then-retract flow
(deffacts startup (item lamp))

(defrule process-item
    ?f <- (item ?x)
    =>
    (retract ?f)
    (assert (processed ?x)))
