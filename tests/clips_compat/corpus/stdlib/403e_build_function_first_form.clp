(defrule run =>
  (printout t (build "(deffunction twice (?x) (* ?x 2)) \"unfinished") ":"
    (eval "(twice 6)") crlf))
