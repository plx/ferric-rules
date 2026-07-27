;; Phase 2 NCC fixture
;; Tests: (not (and ...)) conjunction negation semantics

(deffacts ncc-seed
    (item apple)
    (item banana)
    (block apple)
    (reason apple)
    (block banana))

(defrule allow-when-no-full-block
    (item ?x)
    (not (and (block ?x) (reason ?x)))
    =>
    (assert (allowed ?x)))
