;; A split capture constrains a second multislot in either written order.
(deftemplate item (multislot tags) (multislot expected))
(deffacts input
  (item (tags a marker b marker c) (expected a))
  (item (tags a marker b marker c) (expected a marker b))
  (item (tags a marker b marker c) (expected nope)))
(defglobal ?*forward* = 0 ?*reverse* = 0)
(defrule forward
  (item (tags $?left marker $?right) (expected $?left))
  => (bind ?*forward* (+ ?*forward* 1)))
(defrule reverse
  (item (expected $?left) (tags $?left marker $?right))
  => (bind ?*reverse* (+ ?*reverse* 1)))
(defrule summary (declare (salience -10))
  => (printout t ?*forward* ":" ?*reverse* crlf))
