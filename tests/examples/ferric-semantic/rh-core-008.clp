; RH-CORE-008: named deffacts replacement supplies only the latest definition at reset.
(deffacts seed (version old))
(defrule observe (version ?x) => (printout t ?x crlf) (assert (result ?x)))
