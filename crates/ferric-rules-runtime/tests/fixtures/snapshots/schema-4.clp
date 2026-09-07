; Saved after one of six independent template-slot splits has fired.
(deftemplate bag (slot tag) (multislot left) (multislot right))
(defglobal ?*seen* = 0)
(deffacts bags (bag (tag sample) (left a b) (right c)))
(defrule split
  (bag (tag ?tag) (left $?l1 $?l2) (right $?r1 $?r2))
  =>
  (bind ?*seen* (+ ?*seen* 1))
  (assert (widths (length$ $?l1) (length$ $?l2) (length$ $?r1) (length$ $?r2)))
  (printout t "template split" crlf))
