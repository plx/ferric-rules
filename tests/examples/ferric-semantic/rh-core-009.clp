; RH-CORE-009: replacing one deffacts preserves independent definitions.
(deffacts first (version old))
(deffacts second (sibling retained))
(defrule observe (version ?v) (sibling ?s) => (printout t ?v " " ?s crlf) (assert (result ?v ?s)))
