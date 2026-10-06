;; An overlapping negative and literal alternative matches each fact once.
;; Level: interaction
;; Covers: patterns, field-disjunction, not, ordered-facts
(deffacts seed (sym b) (sym c))
(defrule match (sym ~a|b) => (printout t "match" crlf))
(defrule complete (declare (salience -10)) => (printout t "done" crlf))
