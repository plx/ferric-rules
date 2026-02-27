(defrule foo ; This should fail
   (bbb ?x&:(member a ?x))
   =>)
