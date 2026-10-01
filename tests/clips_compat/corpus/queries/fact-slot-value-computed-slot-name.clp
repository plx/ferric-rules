;; An instance name computed at run time names the slot with its spelling.
;; (A literal one is rejected when CLIPS parses the rule.)
;; Level: boundary
;; Covers: queries, fact-slot-value, instance-name
(deftemplate person (slot name))
(deffacts seed (person (name al)))
(defrule probe ?f <- (person) =>
  (bind ?slot (nth$ 1 (create$ [name])))
  (printout t (fact-slot-value ?f name) " " (fact-slot-value ?f ?slot) crlf))
