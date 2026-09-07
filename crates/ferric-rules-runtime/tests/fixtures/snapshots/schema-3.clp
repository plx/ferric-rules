; Saved after exactly one firing; two positional matches remain pending.
(defglobal ?*seen* = 0)
(deffacts rows (row a b))
(defrule split
  (row $?left $?right)
  =>
  (bind ?*seen* (+ ?*seen* 1))
  (assert (widths (length$ $?left) (length$ $?right)))
  (printout t "split" crlf))
