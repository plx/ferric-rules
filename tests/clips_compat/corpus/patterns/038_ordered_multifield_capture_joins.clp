;; Capture equality must participate in joins in both pattern orders.
(defglobal ?*left-first* = 0 ?*right-first* = 0 ?*tested* = 0)
(deffacts input
  (expected a)
  (expected a marker b)
  (expected missing)
  (row a marker b marker c))
(defrule left-first
  (row $?left marker $?right)
  (expected $?left)
  => (bind ?*left-first* (+ ?*left-first* 1)))
(defrule right-first
  (expected $?left)
  (row $?left marker $?right)
  => (bind ?*right-first* (+ ?*right-first* 1)))
(defrule capture-test
  (row $?left marker $?right)
  (test (= (length$ ?left) 1))
  (test (eq (nth$ 1 ?right) b))
  => (bind ?*tested* (+ ?*tested* 1)))
(defrule summary
  (declare (salience -10))
  => (printout t ?*left-first* ":" ?*right-first* ":" ?*tested* crlf))
