; RH-CORE-002: repeated replacement preserves an independent shared-prefix rule.
(deffacts seed (subject a) (gate open) (proof a))
(defrule choose (subject ?x) (gate open) => (assert (result old ?x)))
(defrule sibling (declare (salience 10)) (subject ?x) (gate open) => (printout t "sibling" crlf) (assert (result sibling ?x)))
