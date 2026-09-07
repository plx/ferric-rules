; RH-CORE-010: deffacts loaded after a reset become active on the next reset.
(defrule observe (late ?v) => (printout t "late " ?v crlf) (assert (result ?v)))
