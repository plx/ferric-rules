(defrule fin
  (foo ?d)
  (test (> ?d 2))
  (foo ?l&~?d)
  =>)
