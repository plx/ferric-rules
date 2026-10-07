;; Reset evaluates template slot expressions in declaration order, not source order.
;; Level: interaction
;; Covers: assertion-expression, deffacts, deftemplate, deffunction, defglobal
(defglobal ?*calls* = 0)
(deffunction next () (bind ?*calls* (+ ?*calls* 1)) ?*calls*)
(deftemplate item (slot id) (slot a) (slot b) (multislot c))
(deffacts seed
  (item (b (next)) (id pair) (a (next)))
  (item (c (next) (next)) (b (next)) (id multi) (a (next))))
(defrule show (item (id ?id) (a ?a) (b ?b) (c $?c))
  => (printout t ?id " a=" ?a " b=" ?b " c=" (implode$ ?c) crlf))
