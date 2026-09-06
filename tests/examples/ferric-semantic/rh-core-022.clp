; RH-CORE-022: asserting a blocker cancels an already queued negative activation.
(deffacts seed (candidate a) (go))
(defrule controller (declare (salience 100)) ?g <- (go) => (retract ?g) (assert (block a)) (assert (result cancelled)))
(defrule blocked (candidate ?x) (not (block ?x)) => (printout t "incorrect" crlf) (assert (result incorrect)))
