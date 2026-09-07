;; Every valid partition is a distinct match, even for anonymous captures.
(defglobal ?*named* = 0 ?*left* = 0 ?*right* = 0 ?*anonymous* = 0 ?*adjacent* = 0 ?*same* = 0)
(deffacts input
  (row a marker b marker c)
  (symmetric a marker a)
  (symmetric a marker b))
(defrule named
  (row $?left marker $?right)
  =>
  (bind ?*named* (+ ?*named* 1))
  (bind ?*left* (+ ?*left* (length$ ?left)))
  (bind ?*right* (+ ?*right* (length$ ?right))))
(defrule anonymous
  (row $? marker $?)
  => (bind ?*anonymous* (+ ?*anonymous* 1)))
(defrule adjacent
  (row $? $?)
  => (bind ?*adjacent* (+ ?*adjacent* 1)))
(defrule repeated-variable
  (symmetric $?same marker $?same)
  => (bind ?*same* (+ ?*same* 1)))
(defrule summary
  (declare (salience -10))
  => (printout t ?*named* ":" ?*left* ":" ?*right* ":" ?*anonymous* ":" ?*adjacent* ":" ?*same* crlf))
