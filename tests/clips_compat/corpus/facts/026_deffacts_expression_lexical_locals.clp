;; Bind destinations and loop-local accessors remain valid seed expressions.
;; Level: interaction
;; Covers: assertion-expression, deffacts, bind, loop-for-count, variable-scope
(deffacts seed
  (bound (bind ?x 3))
  (count (if TRUE then (loop-for-count (?i 1 2) do ?i) 7)))
(defrule show (bound ?bound) (count ?count) => (printout t ?bound ":" ?count crlf))
