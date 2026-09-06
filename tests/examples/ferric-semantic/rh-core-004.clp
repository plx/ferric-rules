; RH-CORE-004: undefrule removes negative activations while a shared sibling remains live.
(deffacts seed (subject a) (remove-now))
(defrule doomed (subject ?x) (not (block ?x)) => (printout t "doomed" crlf))
(defrule survivor (subject ?x) (not (block ?x)) => (printout t "survivor " ?x crlf) (assert (result ?x)))
(defrule controller (declare (salience 100)) ?f <- (remove-now) => (retract ?f) (undefrule doomed) (assert (subject b)))
