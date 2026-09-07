; RH-CORE-005: removed existential rule never reappears when new support is asserted.
(deffacts seed (go) (subject a) (proof a))
(defrule doomed (subject ?x) (exists (proof ?x)) => (printout t "doomed" crlf))
(defrule keep (subject ?x) (exists (proof ?x)) => (printout t "kept " ?x crlf) (assert (result ?x)))
(defrule remove-first (declare (salience 100)) ?g <- (go) ?p <- (proof a) => (undefrule doomed) (retract ?g ?p) (assert (proof a)))
