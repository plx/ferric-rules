(defrule with-error                ; DR0279
   (value ?a&:(> ?a max))
   =>)
(defrule with-error-inside-not     ; DR0279
   (not (value ?b&:(> ?b max)))
   =>)
