;; Phase 2 exists pattern fixture
;; Tests: exists fires at-most-once despite multiple matches
(deffacts data
    (category fruit)
    (item apple fruit)
    (item banana fruit)
    (item cherry fruit))

(defrule has-fruit
    (category fruit)
    (exists (item ?x fruit))
    =>
    (assert (fruit-detected)))
