(defrule should-be-ok
   (message $?first)
   (test (length$ ?first))
   (translation $?first)
   =>)
