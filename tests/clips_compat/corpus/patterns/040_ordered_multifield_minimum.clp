; A fixed field followed by a multifield requires at least one field.
(deffacts input (row) (row a) (row a b) (row a b c))
(defglobal ?*wildcards* = 0 ?*tails* = 0)
(defrule wildcard
  (row ? $?)
  => (bind ?*wildcards* (+ ?*wildcards* 1)))
(defrule named
  (row ?first $?tail)
  => (bind ?*tails* (+ ?*tails* (length$ ?tail))))
(defrule report
  (declare (salience -10))
  => (printout t ?*wildcards* " " ?*tails* crlf))
