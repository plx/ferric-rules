; RH-CORE-031: fact duplication policy can enable then suppress identical assertions.
(deffacts seed (go))
(defrule duplicate-control ?g <- (go) => (retract ?g) (set-fact-duplication TRUE) (assert (item x)) (assert (item x)) (set-fact-duplication FALSE) (assert (item x)))
(defrule count-item (item ?x) => (printout t "item " ?x crlf))
