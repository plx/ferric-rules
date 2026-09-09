; The builder adds raw lexemes and two queued input lines, then runs checkpoint.
(defglobal ?*gate* = TRUE ?*local-calls* = 0 ?*join-calls* = 0
  ?*exists-calls* = 0 ?*cross-calls* = 0 ?*metric-local* = 0
  ?*ordered-seen* = 0 ?*template-seen* = 0 ?*metric-seen* = 0
  ?*captured* = FALSE ?*notice-field* = FALSE ?*last-input* = FALSE)
(deftemplate bag (slot tag) (multislot left) (multislot right))
(deftemplate metric (slot amount))
(deftemplate named (slot name (type INSTANCE-NAME)))
(deffunction eligible (?x)
  (bind ?*local-calls* (+ ?*local-calls* 1)) ?*gate*)
(deffunction blocks (?x ?a)
  (bind ?*join-calls* (+ ?*join-calls* 1)) (= ?x ?a))
(deffunction supports (?x ?a)
  (bind ?*exists-calls* (+ ?*exists-calls* 1)) (> ?x ?a))
(deffunction width-blocks (?x ?prefix)
  (bind ?*cross-calls* (+ ?*cross-calls* 1)) (= ?x (length$ ?prefix)))
(deffunction metric-positive (?x)
  (bind ?*metric-local* (+ ?*metric-local* 1)) (> ?x 0))
(deffunction next-field () (read))
(deffacts inputs
  (data 1) (data 2) (data) (data 1 extra) (anchor 2)
  (owner 2) (support 9) (support 8) (support 9 extra)
  (seq-row a b) (bag (tag sample) (left a b) (right c))
  (width-blocker 2) (seq-outer a b z)
  (metric (amount 3)) (named (name [seed])))
(defrule checkpoint
  (declare (salience 100))
  (raw-payload ?text ?symbol ?name)
  =>
  (bind ?*gate* FALSE)
  (bind ?*captured* (create$ ?text ?symbol ?name))
  (printout t ?text "|" ?symbol "|" ?name crlf)
  (bind ?*notice-field* (string-to-field "9223372036854775808")))
(defrule ordered-split
  (seq-row $?left $?right)
  =>
  (bind ?*ordered-seen* (+ ?*ordered-seen* 1))
  (assert (ordered-widths (length$ $?left) (length$ $?right))))
(defrule template-split
  (bag (tag ?tag) (left $?l1 $?l2) (right $?r1 $?r2))
  =>
  (bind ?*template-seen* (+ ?*template-seen* 1))
  (assert (template-widths (length$ $?l1) (length$ $?l2)
    (length$ $?r1) (length$ $?r2))))
(defrule metric-value
  (metric (amount ?x&:(metric-positive ?x)))
  => (bind ?*metric-seen* (+ ?*metric-seen* 1)))
(defrule absent
  (anchor ?a)
  (not (data ?x&:(eligible ?x)&:(blocks ?x ?a)))
  => (printout t "safe:" ?a crlf))
(defrule supported
  (owner ?a)
  (exists (support ?x&:(supports ?x ?a)))
  (exists-ready)
  => (printout t "supported" crlf))
(defrule captured-outer
  (seq-outer $?prefix ?last)
  (not (width-blocker ?x&:(width-blocks ?x $?prefix)))
  => (assert (outer-width (length$ $?prefix))))
(defrule consume-input
  (take-input ?number)
  =>
  (bind ?*last-input* (next-field))
  (printout t "input:" ?*last-input* crlf))
