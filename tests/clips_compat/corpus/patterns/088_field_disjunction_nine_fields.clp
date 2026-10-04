;; Nine binary field constraints do not require 512 rule variants.
;; Level: interaction
;; Covers: patterns, field-disjunction, ordered-facts, source-limits
(deffacts seed (s a b a b a b a b a) (s a b a b a b a b c))
(defrule match (s a|b a|b a|b a|b a|b a|b a|b a|b a|b)
  => (printout t "match" crlf))
(defrule complete (declare (salience -10)) => (printout t "done" crlf))
