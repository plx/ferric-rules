(defglobal ?*x* = 1)
(deffunction change (?x)
  (bind ?x (+ ?x 1))
  (bind ?*x* (+ ?*x* ?x))
  (create$ ?x ?*x*))
(defrule probe =>
  (printout t (change 2) ":" (change 3) ":" ?*x* crlf))
