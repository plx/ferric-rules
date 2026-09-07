; Saved after one firing; one exact-width match remains pending.
(defglobal ?*seen* = 0)
(deffacts rows (row a) (row b) (row a b))
(defrule observe
  (row ?)
  =>
  (bind ?*seen* (+ ?*seen* 1))
  (printout t "one field" crlf))
