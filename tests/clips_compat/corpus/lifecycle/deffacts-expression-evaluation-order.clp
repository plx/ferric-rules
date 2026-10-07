;; Reset evaluates seed expressions in fact and field order after resetting globals.
;; Level: interaction
;; Covers: assertion-expression, deffacts, reset, deffunction, defglobal
;; Resets: 2
(defglobal ?*calls* = 0)
(deffunction next () (bind ?*calls* (+ ?*calls* 1)) ?*calls*)
(deffacts seed (first (next) (next)) (second (next)))
(defrule show (first ?a ?b) (second ?c)
  => (printout t ?a ":" ?b ":" ?c ":" ?*calls* crlf))
