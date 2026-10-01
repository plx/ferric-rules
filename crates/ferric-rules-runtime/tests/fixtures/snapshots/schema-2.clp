; Saved after exactly one firing; eight positional matches remain pending.
(deftemplate bag (slot tag) (multislot left) (multislot right))
(defglobal ?*seen* = 0)
(deffacts seed
  (row a b)
  (bag (tag sample) (left a b) (right c)))
(defrule row-split
  (row $?left $?right)
  =>
  (bind ?*seen* (+ ?*seen* 1))
  (assert (row-widths (length$ $?left) (length$ $?right))))
(defrule bag-split
  (bag (tag ?tag) (left $?l1 $?l2) (right $?r1 $?r2))
  =>
  (bind ?*seen* (+ ?*seen* 1))
  (assert (bag-widths (length$ $?l1) (length$ $?l2) (length$ $?r1) (length$ $?r2))))
