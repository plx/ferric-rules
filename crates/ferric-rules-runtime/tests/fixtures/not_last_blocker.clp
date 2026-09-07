; RH-CORE-023: correlated not becomes true only after the last distinct blocker is removed.
(deffacts seed (candidate a) (block a first) (block a second) (phase start))
(defrule remove-one (declare (salience 100)) ?p <- (phase start) ?b <- (block a first) => (retract ?p ?b) (assert (phase last)))
(defrule remove-last (declare (salience -10)) ?p <- (phase last) ?b <- (block a second) => (retract ?p ?b) (printout t "unblocked" crlf))
(defrule candidate (candidate ?x) (not (block ?x ?reason)) => (printout t "selected " ?x crlf) (assert (result ?x)))
