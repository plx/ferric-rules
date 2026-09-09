;; Complete captures participate in joins and match-time test CEs.
(deftemplate item (slot id) (multislot tags))
(deftemplate expected (multislot values))
(defglobal ?*left-first* = 0 ?*right-first* = 0 ?*tested* = 0)
(deffacts input
  (expected (values a)) (expected (values a marker b)) (expected (values missing))
  (item (id retained) (tags a marker b marker c)))
(defrule left-first
  (item (tags $?left marker $?right) (id retained))
  (expected (values $?left))
  => (bind ?*left-first* (+ ?*left-first* 1)))
(defrule right-first
  (expected (values $?left))
  (item (id retained) (tags $?left marker $?right))
  => (bind ?*right-first* (+ ?*right-first* 1)))
(defrule capture-test
  (item (tags $?left marker $?right) (id ?id))
  (test (= (length$ ?left) 1))
  (test (eq (nth$ 1 ?right) b))
  (test (eq ?id retained))
  => (bind ?*tested* (+ ?*tested* 1)))
(defrule summary (declare (salience -10))
  => (printout t ?*left-first* ":" ?*right-first* ":" ?*tested* crlf))
