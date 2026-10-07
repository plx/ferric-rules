; Saved after exactly one firing; eight positional matches remain pending.
; Seed expressions stay executable for subsequent resets after restoration.
(deftemplate bag (slot tag) (multislot left) (multislot right))
(defglobal ?*seen* = 0 ?*seed-number* = 7 ?*fixture-address* = FALSE)
(deffunction seed-fields () (create$ a b))
(deffacts seed
  (row (seed-fields))
  (seed-number (if FALSE then (assert (unused-effect 1)) else (+ ?*seed-number* 1)))
  (bag (tag (sym-cat sam ple)) (left (create$ a b)) (right (create$ c))))
(defrule row-split
  ?row <- (row $?left $?right)
  =>
  (bind ?*fixture-address* ?row)
  (bind ?*seen* (+ ?*seen* 1))
  (assert (row-widths (length$ $?left) (length$ $?right))))
(defrule bag-split
  ?bag <- (bag (tag ?tag) (left $?l1 $?l2) (right $?r1 $?r2))
  =>
  (bind ?*fixture-address* ?bag)
  (bind ?*seen* (+ ?*seen* 1))
  (assert (bag-widths (length$ $?l1) (length$ $?l2) (length$ $?r1) (length$ $?r2))))

; Dormant until after restore; y satisfies both alternatives but fires once.
(defrule field-alternative
  (choice ?value&~x|y)
  =>
  (assert (accepted ?value)))
(defrule sequence-alternative
  (choices $? ?value&~x|y $?)
  =>
  (assert (accepted-sequence ?value)))

; Persist both fixed-parameter queries and typed/query-restricted wildcards.
(defmethod fixture-method (?key $?rest) -1)
(defmethod fixture-method ((?key SYMBOL (eq ?key special))
                           ($?rest SYMBOL (> (length$ ?rest) 0)))
  (length$ ?rest))

; The first firing retains its supporting fact address in a global.

; Dormant expression effects must remain executable after restoration.
(deffunction fixture-mutate (?value) (assert (effect-created ?value)))
(defmethod fixture-effect-method ((?value INTEGER)) (assert (effect-method ?value)))
(defrule resume-effect
  (fixture-effect ?value)
  =>
  (bind ?made (fixture-mutate ?value))
  (printout t (fact-existp ?made) "|"
    (fact-existp (fixture-effect-method ?value)) "|"
    (do-for-fact ((?bag bag)) TRUE (length$ ?bag:left)) crlf)
  (retract ?made))
