;; Phase 2 negative pattern fixture
;; Tests: negative pattern blocking/unblocking, variable binding in negation
(deffacts items
    (item apple)
    (item banana)
    (item cherry))

(deffacts restrictions
    (forbidden banana))

(defrule allowed-items
    (item ?name)
    (not (forbidden ?name))
    =>
    (assert (allowed ?name)))
