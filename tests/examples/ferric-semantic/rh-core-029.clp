; RH-CORE-029: duplicating a template with overrides preserves the original fact.
(deftemplate entry (slot id) (slot value))
(deffacts seed (entry (id original) (value 4)))
(defrule copy ?e <- (entry (id original)) => (duplicate ?e (id copy) (value 8)))
(defrule verify (entry (id original) (value ?a)) (entry (id copy) (value ?b)) => (printout t ?a " " ?b crlf) (assert (result ?a ?b)))
