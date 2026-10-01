;; Three adjacent captures enumerate every weak partition of the fields.
(deffacts input (row a b))
(defrule observe
  (row $?left $?middle $?right)
  => (printout t (length$ ?left) ":" (length$ ?middle) ":" (length$ ?right) " " ?left "|" ?middle "|" ?right crlf))
