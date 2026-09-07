; RH-CORE-003: failed new rule load leaves a previously compiled sibling intact.
(deffacts seed (subject ok))
(defrule survivor (subject ?x) => (printout t "survivor " ?x crlf) (assert (result ?x)))
