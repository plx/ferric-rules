;; Phase 2 retract fixture
;; Tests: fact-address variables and retract action
(deffacts startup
    (temporary data))

(defrule cleanup
    ?f <- (temporary ?x)
    =>
    (retract ?f)
    (assert (cleaned ?x)))
