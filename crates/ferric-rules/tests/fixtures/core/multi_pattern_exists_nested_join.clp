; Nested multi-pattern exists join pinned against CLIPS 6.30.
;
; Two complete three-pattern tuples satisfy the existential CE, but the rule
; must still receive exactly one activation. The tuple-local test is evaluated
; before the existential result is collapsed to Boolean support.

(deffacts startup
    (a one)
    (b one 10)
    (c 10)
    (a two)
    (b two 20)
    (c 20))

(defrule nested-join
    (exists
        (a ?x)
        (b ?x ?y)
        (test (> ?y 0))
        (c ?y))
    =>
    (printout t "nested" crlf))
