; RH-CORE-030: retraction cancels dependent joined activations while preserving other candidates.
(deffacts seed (item a) (item b) (enabled a) (enabled b) (go))
(defrule retract-a (declare (salience 100)) ?g <- (go) ?e <- (enabled a) => (retract ?g ?e))
(defrule ready (item ?x) (enabled ?x) => (printout t "ready " ?x crlf) (assert (result ?x)))
