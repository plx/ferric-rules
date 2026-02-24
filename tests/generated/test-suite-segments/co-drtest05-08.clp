(defrule erroneous-syntax-error    ; DR0451
   (fact1 test ?symbol&:(eq ?symbol :) ?num)
   =>)
