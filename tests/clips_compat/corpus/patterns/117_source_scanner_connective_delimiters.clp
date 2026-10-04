;; Numeric fields terminate at compact constraint connectives.
;; Level: boundary
;; Covers: source-scanner, deffacts, field-disjunction, variable-binding
(deffacts seed (number .5) (number 1.) (number 2.))
(defrule match (number ?value&.5|1.) => (printout t "match " ?value crlf))
(defrule other (declare (salience -10)) (number ?value&~.5&~1.)
  => (printout t "other " ?value crlf))
