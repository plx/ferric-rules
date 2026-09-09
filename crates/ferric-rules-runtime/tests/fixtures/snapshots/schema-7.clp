;; Run one activation before saving: retained local decisions precede gate change.
(defglobal ?*local-calls* = 0 ?*join-calls* = 0 ?*gate* = TRUE)
(deffunction eligible (?x)
  (bind ?*local-calls* (+ ?*local-calls* 1))
  ?*gate*)
(deffunction blocks (?x ?a)
  (bind ?*join-calls* (+ ?*join-calls* 1))
  (= ?x ?a))
(deffacts inputs (data 1) (data 2) (anchor 2))
(defrule gate-change
  (declare (salience 100))
  => (bind ?*gate* FALSE))
(defrule absent
  (anchor ?a)
  (not (data ?x&:(eligible ?x)&:(blocks ?x ?a)))
  => (printout t "safe:" ?a crlf))
