;; An existential ordered sequence matches multiple alternatives only once.
;; Level: interaction
;; Covers: patterns, field-disjunction, exists, ordered-facts, multifield
(deffacts seed (go) (tags a b))
(defrule match (go) (exists (tags $? a|b $?))
  => (printout t "match" crlf))
(defrule complete (declare (salience -10)) => (printout t "done" crlf))
